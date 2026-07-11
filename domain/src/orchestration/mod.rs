//! Quorum orchestration domain
//!
//! This module contains the core orchestration data model for running
//! multi-model discussions. Configuration is expressed through three
//! orthogonal axes:
//!
//! - [`mode::ConsensusLevel`] — **Solo** (single model) or **Ensemble** (multi-model)
//! - [`scope::PhaseScope`] — **Full**, **Fast** (skip reviews), or **PlanOnly**
//! - [`strategy::OrchestrationStrategy`] — **Quorum** or **Debate** (each carrying its own config)
//!
//! These axes are independent and can be freely combined.
//!
//! Executing a strategy (querying models, running rounds, reporting progress) is an
//! application-layer concern — see `StrategyExecutor` and its implementations
//! (`QuorumStrategyExecutor`, `DebateStrategyExecutor`) in
//! `application/src/use_cases/run_quorum/`. It depends on the `LlmGateway`/
//! `ProgressNotifier` ports, which domain must not depend on.

pub mod entities;
pub mod mode;
pub mod scope;
pub mod session_mode;
pub mod strategy;
pub mod stream_context;
pub mod value_objects;
