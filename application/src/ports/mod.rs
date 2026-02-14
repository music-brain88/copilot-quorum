//! Port definitions (interfaces for external adapters)
//!
//! Ports define the contracts that infrastructure adapters must implement.

pub mod action_reviewer;
pub mod agent_progress;
pub mod context_loader;
pub mod human_intervention;
pub mod llm_gateway;
pub mod progress;
pub mod reference_resolver;
pub mod tool_executor;
pub mod ui_event;
