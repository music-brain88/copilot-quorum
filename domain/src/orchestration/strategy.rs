//! Orchestration strategy trait
//!
//! Different strategies can be plugged in to change how the Quorum
//! discussion is orchestrated.

use crate::core::error::DomainError;
use crate::core::model::Model;
use crate::core::question::Question;
use crate::orchestration::entities::Phase;
use crate::orchestration::value_objects::QuorumResult;
use async_trait::async_trait;

/// Callback for progress updates during orchestration
pub trait ProgressNotifier: Send + Sync {
    /// Called when a phase starts
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize);

    /// Called when a task completes within a phase
    fn on_task_complete(&self, phase: &Phase, model: &Model, success: bool);

    /// Called when a phase completes
    fn on_phase_complete(&self, phase: &Phase);
}

/// No-op progress notifier
pub struct NoProgress;

impl ProgressNotifier for NoProgress {
    fn on_phase_start(&self, _phase: &Phase, _total_tasks: usize) {}
    fn on_task_complete(&self, _phase: &Phase, _model: &Model, _success: bool) {}
    fn on_phase_complete(&self, _phase: &Phase) {}
}

/// Gateway trait for LLM communication
///
/// This is defined in the domain layer but implemented in infrastructure.
/// It provides the interface for orchestration strategies to communicate with LLMs.
#[async_trait]
pub trait LlmGateway: Send + Sync {
    /// Error type for gateway operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Send a query to a model and get a response
    async fn query(
        &self,
        model: &Model,
        system_prompt: Option<&str>,
        user_prompt: &str,
    ) -> Result<String, Self::Error>;
}

/// Trait for orchestration strategies
///
/// Different strategies implement different flows for the Quorum discussion.
/// Examples:
/// - ThreePhaseStrategy: Initial → Review → Synthesis
/// - FastStrategy: Initial → Synthesis (no review)
/// - DebateStrategy: Models discuss and debate with each other
#[async_trait]
pub trait OrchestrationStrategy: Send + Sync {
    /// Get the name of this strategy
    fn name(&self) -> &'static str;

    /// Get the phases this strategy will execute
    fn phases(&self) -> Vec<Phase>;

    /// Execute the orchestration strategy
    async fn execute<G: LlmGateway>(
        &self,
        question: &Question,
        models: &[Model],
        moderator: &Model,
        gateway: &G,
        notifier: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, DomainError>;
}
