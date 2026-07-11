//! Run Quorum use case
//!
//! Dispatches a Quorum discussion to the [`StrategyExecutor`] matching
//! `input.strategy`. The dispatch is an **exhaustive match** on
//! [`OrchestrationStrategy`] (not `dyn StrategyExecutor` trait-object dispatch) —
//! see [`strategy_executor`] module docs for why, and #314 for the extraction
//! that introduced this shape (previously the Quorum flow was hardcoded here).

mod debate_strategy;
mod quorum_strategy;
mod strategy_executor;
#[cfg(test)]
mod test_support;
mod types;

pub use debate_strategy::DebateStrategyExecutor;
pub use quorum_strategy::QuorumStrategyExecutor;
pub use strategy_executor::StrategyExecutor;
pub use types::{RunQuorumError, RunQuorumInput};

use crate::ports::llm_gateway::LlmGateway;
use crate::ports::progress::{NoProgress, ProgressNotifier};
use quorum_domain::{OrchestrationStrategy, QuorumResult};
use std::sync::Arc;
use tracing::info;

/// Use case for running a Quorum discussion
pub struct RunQuorumUseCase {
    gateway: Arc<dyn LlmGateway>,
}

impl RunQuorumUseCase {
    pub fn new(gateway: Arc<dyn LlmGateway>) -> Self {
        Self { gateway }
    }

    /// Execute the use case with default (no-op) progress
    pub async fn execute(&self, input: RunQuorumInput) -> Result<QuorumResult, RunQuorumError> {
        self.execute_with_progress(input, &NoProgress).await
    }

    /// Execute the use case with progress callbacks
    pub async fn execute_with_progress(
        &self,
        input: RunQuorumInput,
        progress: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, RunQuorumError> {
        if input.models.participants.is_empty() {
            return Err(RunQuorumError::NoModels);
        }

        info!(
            "Starting Quorum ({}) with {} models",
            input.strategy.name(),
            input.models.participants.len()
        );

        let gateway = Arc::clone(&self.gateway);
        match &input.strategy {
            OrchestrationStrategy::Quorum(_) => {
                QuorumStrategyExecutor::new()
                    .execute(&input, gateway, progress)
                    .await
            }
            OrchestrationStrategy::Debate(_) => {
                DebateStrategyExecutor::new()
                    .execute(&input, gateway, progress)
                    .await
            }
        }
    }
}
