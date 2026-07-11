//! `StrategyExecutor` — the application-layer trait executing one
//! [`OrchestrationStrategy`](quorum_domain::OrchestrationStrategy) variant's discussion flow.
//!
//! This lives here (not in domain) because executing a strategy means talking to
//! LLMs and reporting progress — both application-layer ports
//! ([`LlmGateway`], [`ProgressNotifier`]). [`RunQuorumUseCase`](super::RunQuorumUseCase)
//! dispatches to implementations via an **exhaustive match** on `OrchestrationStrategy`,
//! not dynamic `dyn StrategyExecutor` dispatch — so this trait doesn't need to support
//! trait objects for that purpose. It's still written to be object-safe (no generic
//! methods) since that costs nothing and keeps the door open for callers that do want
//! dynamic dispatch.
//!
//! `gateway` is an owned `Arc` (not `&dyn LlmGateway`) because implementations may need
//! to clone it into `'static` spawned tasks for parallel model queries (see
//! [`QuorumStrategyExecutor`](super::quorum_strategy::QuorumStrategyExecutor)).

use super::types::{RunQuorumError, RunQuorumInput};
use crate::ports::llm_gateway::LlmGateway;
use crate::ports::progress::ProgressNotifier;
use async_trait::async_trait;
use quorum_domain::{Phase, QuorumResult};
use std::sync::Arc;

/// Executes one orchestration strategy's discussion flow end-to-end.
#[async_trait]
pub trait StrategyExecutor: Send + Sync {
    /// Name of this executor, for logging/diagnostics.
    fn name(&self) -> &'static str;

    /// Phases this executor reports through [`ProgressNotifier`].
    fn phases(&self) -> Vec<Phase>;

    /// Execute the strategy and produce a [`QuorumResult`].
    async fn execute(
        &self,
        input: &RunQuorumInput,
        gateway: Arc<dyn LlmGateway>,
        progress: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, RunQuorumError>;
}
