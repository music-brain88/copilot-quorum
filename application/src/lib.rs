//! Application layer for copilot-quorum
//!
//! This crate contains use cases, port definitions, and application configuration.
//! It depends only on the domain layer.

pub mod config;
pub mod ports;
pub mod use_cases;

// Re-export commonly used types
pub use config::BehaviorConfig;
pub use ports::{
    context_loader::ContextLoaderPort,
    human_intervention::{
        AutoApproveIntervention, AutoRejectIntervention, HumanInterventionError,
        HumanInterventionPort,
    },
    llm_gateway::{LlmGateway, StreamHandle},
    progress::ProgressNotifier,
    tool_executor::ToolExecutorPort,
};
pub use use_cases::init_context::{
    InitContextError, InitContextInput, InitContextOutput, InitContextProgressNotifier,
    InitContextUseCase, NoInitContextProgress,
};
pub use use_cases::run_agent::{
    AgentProgressNotifier, ErrorCategory, NoAgentProgress, RunAgentError, RunAgentInput,
    RunAgentOutput, RunAgentUseCase,
};
pub use use_cases::run_quorum::{RunQuorumInput, RunQuorumUseCase};
