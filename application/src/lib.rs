//! Application layer for copilot-quorum
//!
//! This crate contains use cases, port definitions, and application configuration.
//! It depends only on the domain layer.

pub mod config;
pub mod ports;
pub mod use_cases;

// Re-export commonly used types
pub use config::ExecutionParams;
pub use config::QuorumConfig;
pub use ports::agent_progress::{AgentProgressNotifier, NoAgentProgress};
pub use ports::config_accessor::{ConfigAccessError, ConfigAccessorPort, ConfigValue};
pub use ports::conversation_logger::{ConversationEvent, ConversationLogger, NoConversationLogger};
pub use ports::{
    context_loader::ContextLoaderPort,
    human_intervention::{
        AutoApproveIntervention, AutoRejectIntervention, HumanInterventionError,
        HumanInterventionPort,
    },
    llm_gateway::{LlmGateway, StreamHandle},
    progress::ProgressNotifier,
    reference_resolver::{ReferenceError, ReferenceResolverPort, ResolvedReference},
    tool_executor::ToolExecutorPort,
    tool_schema::ToolSchemaPort,
};
pub use use_cases::init_context::{
    InitContextError, InitContextInput, InitContextOutput, InitContextProgressNotifier,
    InitContextUseCase, NoInitContextProgress,
};
pub use use_cases::run_agent::{RunAgentError, RunAgentInput, RunAgentOutput, RunAgentUseCase};
pub use use_cases::run_ask::{RunAskError, RunAskInput, RunAskUseCase};
// Re-export ErrorCategory from domain (was previously in run_agent)
pub use quorum_domain::ErrorCategory;
pub use use_cases::run_quorum::{RunQuorumInput, RunQuorumUseCase};

// Extracted use cases (Phase 1 + Phase 4)
pub use ports::action_reviewer::{ActionReviewer, ReviewDecision};
pub use use_cases::execute_task::ExecuteTaskUseCase;
pub use use_cases::gather_context::GatherContextUseCase;

// UI event types (output port for presentation layer)
pub use ports::ui_event::{
    AgentErrorEvent, AgentResultEvent, AskResultEvent, ConfigSnapshot, ContextInitResultEvent,
    InteractionCompletedEvent, InteractionSpawnedEvent, QuorumResultEvent, UiEvent, WelcomeInfo,
};

// Agent controller
pub use use_cases::agent_controller::{AgentController, CommandAction};
