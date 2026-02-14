//! Quorum orchestration domain
//!
//! This module contains the core orchestration logic for running
//! multi-model discussions. Configuration is expressed through three
//! orthogonal axes:
//!
//! - [`mode::ConsensusLevel`] — **Solo** (single model) or **Ensemble** (multi-model)
//! - [`scope::PhaseScope`] — **Full**, **Fast** (skip reviews), or **PlanOnly**
//! - [`strategy::OrchestrationStrategy`] — **Quorum** or **Debate** (each carrying its own config)
//!
//! These axes are independent and can be freely combined.
//!
//! [`strategy::StrategyExecutor`] is the trait that concrete strategy
//! implementations use to execute a discussion flow against LLM models.

pub mod entities;
pub mod mode;
pub mod scope;
pub mod session_mode;
pub mod strategy;
pub mod value_objects;
