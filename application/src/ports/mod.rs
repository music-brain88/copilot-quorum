//! Port definitions (interfaces for external adapters)
//!
//! Ports define the contracts that infrastructure adapters must implement.

pub mod action_reviewer;
pub mod agent_progress;
pub mod composite_progress;
pub mod config_accessor;
pub mod context_loader;
pub mod conversation_logger;
pub mod human_intervention;
pub mod llm_gateway;
pub mod progress;
pub mod reference_resolver;
pub mod script_progress_bridge;
pub mod scripting_engine;
pub mod tool_executor;
pub mod tool_schema;
pub mod tui_accessor;
pub mod tui_accessor_state;
pub mod ui_event;
